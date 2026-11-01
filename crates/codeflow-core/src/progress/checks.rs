//! Check runs and the required-check view.
//!
//! The pull request list gives every run but not which checks are
//! required. The required set is read per base branch from the forge's
//! rules and branch protection, once each, through the bounded observer.
//! A set that cannot be read is an error with its reason, never an empty
//! set, so a missing required check is never reported as none required.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use super::inputs::CheckRunIn;

/// States of a run that has not finished.
const PENDING: [&str; 6] = [
    "pending",
    "queued",
    "in_progress",
    "expected",
    "waiting",
    "requested",
];

/// States that are neither a failure nor unfinished.
const SETTLED: [&str; 3] = ["success", "neutral", "skipped"];

/// The summary of `runs`: `failed` when any failed, was cancelled or timed
/// out, else `pending` when any has not finished, `missing` when none ran,
/// `skipped` when any skipped (a skip is not a pass without reuse
/// evidence), else `passed`.
pub(super) fn summary<'a>(runs: impl IntoIterator<Item = &'a CheckRunIn>) -> &'static str {
    let states: Vec<&str> = runs.into_iter().map(|run| run.state.as_str()).collect();
    if states.is_empty() {
        "missing"
    } else if states
        .iter()
        .any(|state| !SETTLED.contains(state) && !PENDING.contains(state))
    {
        "failed"
    } else if states.iter().any(|state| PENDING.contains(state)) {
        "pending"
    } else if states.contains(&"skipped") {
        "skipped"
    } else {
        "passed"
    }
}

/// The required view of a pull request's runs: `unverified` when the
/// required set could not be read, `none required` when the base requires
/// none, else the worst of the required checks, where a required check
/// with no run is `missing`.
pub(super) fn required_summary(
    runs: &[CheckRunIn],
    required: &Result<Vec<String>, String>,
) -> String {
    let Ok(names) = required else {
        return "unverified".into();
    };
    if names.is_empty() {
        return "none required".into();
    }
    let per_name: Vec<&'static str> = names
        .iter()
        .map(|name| summary(runs.iter().filter(|run| &run.name == name)))
        .collect();
    ["failed", "pending", "missing", "skipped"]
        .into_iter()
        .find(|worst| per_name.contains(worst))
        .unwrap_or("passed")
        .into()
}

/// The most rules one read lists; a full page may hide more.
const RULES_LIMIT: usize = 100;

/// The check names `base` requires on `repository`, from its rulesets and
/// its branch protection, or why they could not be read.
pub(super) fn required_names(
    root: &Path,
    gh: &std::ffi::OsStr,
    limit: Duration,
    repository: &str,
    base: &str,
) -> Result<Vec<String>, String> {
    if base.is_empty()
        || !base
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._/-".contains(&byte))
    {
        return Err(format!(
            "the base branch `{base}` is not a name status reads required checks for"
        ));
    }
    let read = |path: String| -> Result<Value, String> {
        let mut command = crate::git::process(gh);
        command
            .current_dir(root)
            .env("GH_PROMPT_DISABLED", "1")
            .args(["api", &path]);
        let output = crate::coord::observe::command(command, limit).ok_or_else(|| {
            "the required-check read did not finish within its bound or could not start".to_string()
        })?;
        if !output.status.success() {
            return Err("the required-check read failed (offline or not signed in)".into());
        }
        serde_json::from_slice(&output.stdout)
            .map_err(|_| "the required-check answer was not the expected JSON".to_string())
    };
    let malformed = || "the required-check answer was not the expected JSON".to_string();
    let mut names: Vec<String> = Vec::new();
    let rules = read(format!(
        "repos/{repository}/rules/branches/{base}?per_page={RULES_LIMIT}"
    ))?;
    let rules = rules.as_array().ok_or_else(malformed)?;
    if rules.len() >= RULES_LIMIT {
        return Err(format!(
            "the forge listed {} rules for `{base}`, its read limit, so more may exist",
            rules.len()
        ));
    }
    for rule in rules {
        let kind = rule
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(malformed)?;
        if kind != "required_status_checks" {
            continue;
        }
        let checks = rule
            .pointer("/parameters/required_status_checks")
            .and_then(Value::as_array)
            .ok_or_else(malformed)?;
        for check in checks {
            let context = check
                .get("context")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?;
            names.push(context.to_string());
        }
    }
    let branch = read(format!("repos/{repository}/branches/{base}"))?;
    if branch.get("name").and_then(Value::as_str) != Some(base) {
        return Err(malformed());
    }
    if let Some(protection) = branch.pointer("/protection/required_status_checks") {
        let contexts = protection.get("contexts").and_then(Value::as_array);
        let checks = protection.get("checks").and_then(Value::as_array);
        if contexts.is_none() && checks.is_none() {
            return Err(malformed());
        }
        for context in contexts.into_iter().flatten() {
            names.push(context.as_str().ok_or_else(malformed)?.to_string());
        }
        for check in checks.into_iter().flatten() {
            let context = check
                .get("context")
                .and_then(Value::as_str)
                .ok_or_else(malformed)?;
            names.push(context.to_string());
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

/// An RFC 3339 time with any offset and optional fraction, as seconds since
/// the Unix epoch; `None` when it is not such a time.
pub(super) fn parse_instant(text: &str) -> Option<i64> {
    let (date_time, rest) = (text.get(..19)?, text.get(19..)?);
    let rest = match rest.strip_prefix('.') {
        Some(fraction) => {
            let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
            if digits == 0 {
                return None;
            }
            &fraction[digits..]
        }
        None => rest,
    };
    let base = i64::try_from(crate::coord::parse_time(&format!("{date_time}Z")).ok()?).ok()?;
    let offset = match rest {
        "Z" | "z" => 0,
        _ => {
            let bytes = rest.as_bytes();
            if bytes.len() != 6 || bytes[3] != b':' {
                return None;
            }
            let sign = match bytes[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hours: i64 = rest.get(1..3)?.parse().ok()?;
            let minutes: i64 = rest.get(4..6)?.parse().ok()?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            sign * (hours * 3600 + minutes * 60)
        }
    };
    Some(base - offset)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(name: &str, state: &str) -> CheckRunIn {
        CheckRunIn {
            name: name.into(),
            state: state.into(),
            completed_at: None,
            required: None,
        }
    }

    #[test]
    fn a_required_check_with_no_run_is_missing_and_optional_success_does_not_hide_it() {
        let runs = [run("optional-lint", "success")];
        assert_eq!(
            required_summary(&runs, &Ok(vec!["build".into()])),
            "missing"
        );
        assert_eq!(required_summary(&runs, &Ok(Vec::new())), "none required");
        assert_eq!(required_summary(&runs, &Err("unread".into())), "unverified");
        let runs = [run("build", "success"), run("lint", "failure")];
        assert_eq!(required_summary(&runs, &Ok(vec!["build".into()])), "passed");
        assert_eq!(summary(&runs), "failed");
    }

    #[test]
    fn offsets_and_fractions_normalize_to_utc() {
        assert_eq!(
            parse_instant("2026-11-01T07:00:00-04:00"),
            parse_instant("2026-11-01T11:00:00Z")
        );
        assert_eq!(
            parse_instant("2026-11-01T11:00:00.250Z"),
            parse_instant("2026-11-01T11:00:00Z")
        );
        assert_eq!(parse_instant("yesterday"), None);
        assert_eq!(parse_instant("2026-11-01T11:00:00+2:00"), None);
    }
}
