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
use crate::scaffold::sha256_hex;

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
    let installed = match super::read_installed_manifest(&root) {
        Ok(installed) => installed,
        Err(error) => {
            let path = crate::scaffold::state::InstalledManifest::path(&root);
            return input_failure(NAME, start, &path, error);
        }
    };
    let mut applied = Vec::new();
    let mut pending = Vec::new();
    let mut differs = Vec::new();
    for (dest, dest_plan) in &plan.dests {
        let installed_sha = installed.files.get(dest).map(|file| file.sha256.as_str());
        match dest_state(&root, dest, dest_plan, installed_sha) {
            Ok(State::Applied) => {}
            Ok(State::Pending) => pending.push(dest.clone()),
            Ok(State::Differs) => differs.push(dest.clone()),
            Err(error) => return input_failure(NAME, start, &root.join(dest), error),
        }
        for unit in &dest_plan.applied {
            applied.push(describe(dest, unit));
        }
    }
    if !differs.is_empty() {
        return result(
            Status::Fail,
            format!(
                "{} holds text that is neither the shipped text with the declared overrides nor what \
                 `codeflow update` last wrote and recorded: an undeclared edit or unverified state, \
                 which no override accounts for; `codeflow update` merges it or writes a .new proposal",
                differs.join(", ")
            ),
        );
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

/// Where one overridden file stands.
enum State {
    /// The file is the shipped text with the declared overrides, as this
    /// binary computes it, and update's record says so.
    Applied,
    /// The file is what update last wrote and recorded, or the shipped text
    /// before any override: `codeflow update` applies the declared set.
    Pending,
    /// Anything else.
    Differs,
}

/// The state of `dest`. Every comparison is with text this binary computes:
/// the effective rendering from its shipped text and the declared overrides,
/// and the effective baseline update's reconstruction rebuilt from the
/// record and checked against the baseline and `installed_sha`. A digest the
/// project wrote counts only when it equals that computation.
fn dest_state(
    root: &Path,
    dest: &str,
    plan: &DestPlan,
    installed_sha: Option<&str>,
) -> Result<State, crate::scaffold::ScaffoldError> {
    let Some(current) =
        crate::scaffold::state::read_beneath_root(root, dest)?.and_then(|text| plan.unit_of(&text))
    else {
        return Ok(State::Differs);
    };
    let expected = plan.effective_unit();
    if expected.as_deref() == Some(current.as_str()) {
        let recorded = match &plan.old {
            Some((record, _)) => {
                record.applied == plan.applied
                    && record.effective_sha256 == sha256_hex(current.as_bytes())
            }
            None => plan.applied.is_empty() && record::Record::read(root, dest)?.is_none(),
        };
        return Ok(if recorded {
            State::Applied
        } else {
            State::Pending
        });
    }
    let last = match &plan.old {
        Some((_, effective)) => effective == &current,
        None => Some(sha256_hex(current.as_bytes()).as_str()) == installed_sha,
    };
    Ok(if last { State::Pending } else { State::Differs })
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
